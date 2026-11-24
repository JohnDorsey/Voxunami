//! The joint desk: what the command `/joint decouple` and the plugin that couples ships tell each other (`docs/decisions/joints-and-cars.md`, "The command"). It is a shared
//! record and has no rules of its own beyond which coupling of a ship is nearest a place, and which pairs are held apart until they have come apart.

use std::collections::BTreeMap;

use grid_bodies::JointDesk;
use voxel_space::{BrickCoordinate, GridId, LevelIndex, WorldPosition};

fn ship(number: u64) -> GridId {
    GridId::new(GridId::FIRST_ALLOCATED + number)
}

fn at(x: f64, y: f64, z: f64) -> WorldPosition {
    WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z])
}

/// Ships 0, 1, 2 and 3 in a chain: the pivots of 0-1, 1-2 and 2-3 are at x = 10, 20 and 30 (and a pair of 5 and 6 far off on its own, at x = 500).
fn a_chain() -> JointDesk {
    let desk = JointDesk::default();
    desk.set_pivots(BTreeMap::from([
        ((ship(0), ship(1)), (LevelIndex::MAIN, at(10.0, 0.0, 0.0))),
        ((ship(1), ship(2)), (LevelIndex::MAIN, at(20.0, 0.0, 0.0))),
        ((ship(2), ship(3)), (LevelIndex::MAIN, at(30.0, 0.0, 0.0))),
        ((ship(5), ship(6)), (LevelIndex::MAIN, at(500.0, 0.0, 0.0))),
    ]));
    desk
}

fn pairs_of(found: Vec<(f64, (GridId, GridId))>) -> Vec<(GridId, GridId)> {
    found.into_iter().map(|(_, pair)| pair).collect()
}

#[test]
fn the_couplings_within_reach_of_a_place_come_nearest_first_with_how_far_each_is() {
    let desk = a_chain();
    let found = desk.couplings_within(LevelIndex::MAIN, at(18.0, 0.0, 0.0), 15.0);
    assert_eq!(
        found
            .iter()
            .map(|(distance, _)| *distance)
            .collect::<Vec<_>>(),
        vec![2.0, 8.0, 12.0]
    );
    assert_eq!(
        pairs_of(found),
        vec![(ship(1), ship(2)), (ship(0), ship(1)), (ship(2), ship(3))]
    );
    // Reach is a limit, and the far pair is out of it however the others are.
    assert_eq!(
        pairs_of(desk.couplings_within(LevelIndex::MAIN, at(18.0, 0.0, 0.0), 8.0)),
        vec![(ship(1), ship(2)), (ship(0), ship(1))]
    );
    assert!(
        desk.couplings_within(LevelIndex::MAIN, at(18.0, 0.0, 0.0), 1.0)
            .is_empty()
    );
}

#[test]
fn a_tie_is_the_pair_first_in_order_so_that_what_is_asked_twice_is_answered_twice_the_same() {
    let desk = a_chain();
    assert_eq!(
        pairs_of(desk.couplings_within(LevelIndex::MAIN, at(15.0, 0.0, 0.0), 6.0)),
        vec![(ship(0), ship(1)), (ship(1), ship(2))]
    );
}

#[test]
fn a_coupling_in_another_level_is_not_near_whatever_its_numbers_say() {
    let desk = JointDesk::default();
    desk.set_pivots(BTreeMap::from([
        ((ship(0), ship(1)), (LevelIndex::new(1), at(10.0, 0.0, 0.0))),
        ((ship(2), ship(3)), (LevelIndex::MAIN, at(12.0, 0.0, 0.0))),
    ]));
    assert_eq!(
        pairs_of(desk.couplings_within(LevelIndex::MAIN, at(10.0, 0.0, 0.0), 100.0)),
        vec![(ship(2), ship(3))]
    );
    assert_eq!(
        pairs_of(desk.couplings_within(LevelIndex::new(1), at(10.0, 0.0, 0.0), 100.0)),
        vec![(ship(0), ship(1))]
    );
}

#[test]
fn nothing_is_near_before_the_plugin_has_told_a_pivot() {
    assert!(
        JointDesk::default()
            .couplings_within(LevelIndex::MAIN, at(10.0, 0.0, 0.0), 1.0e9)
            .is_empty()
    );
}

#[test]
fn the_pivots_the_plugin_tells_replace_the_ones_it_told_before() {
    let desk = a_chain();
    desk.set_pivots(BTreeMap::from([(
        (ship(0), ship(1)),
        (LevelIndex::MAIN, at(11.0, 0.0, 0.0)),
    )]));
    assert_eq!(
        pairs_of(desk.couplings_within(LevelIndex::MAIN, at(20.0, 0.0, 0.0), 100.0)),
        vec![(ship(0), ship(1))]
    );
}

#[test]
fn a_request_is_taken_once_and_a_pair_asked_for_twice_is_asked_for_once() {
    let desk = a_chain();
    desk.decouple((ship(0), ship(1)));
    desk.decouple((ship(0), ship(1)));
    desk.decouple((ship(2), ship(3)));
    let taken = desk.take_requests();
    assert_eq!(
        taken.into_iter().collect::<Vec<_>>(),
        vec![(ship(0), ship(1)), (ship(2), ship(3))]
    );
    assert!(desk.take_requests().is_empty());
}

#[test]
fn the_desk_that_a_clone_has_is_the_same_desk() {
    let desk = a_chain();
    let other = desk.clone();
    other.decouple((ship(1), ship(2)));
    assert_eq!(
        desk.take_requests().into_iter().collect::<Vec<_>>(),
        vec![(ship(1), ship(2))]
    );
    desk.hold_apart((ship(1), ship(2)));
    assert!(other.is_held_apart((ship(1), ship(2))));
    assert!(!other.is_held_apart((ship(0), ship(1))));
    other.let_go_of((ship(1), ship(2)));
    assert!(!desk.is_held_apart((ship(1), ship(2))));
}

#[test]
fn a_lock_that_is_wanted_is_wanted_until_it_is_cancelled_and_is_not_taken_as_a_request_is() {
    let desk = a_chain();
    desk.lock((ship(2), ship(3)));
    desk.lock((ship(0), ship(1)));
    desk.lock((ship(0), ship(1)));
    assert!(desk.is_lock_wanted((ship(0), ship(1))));
    assert!(!desk.is_lock_wanted((ship(1), ship(2))));
    // In order of pair, and asking does not take them.
    let wanted = vec![(ship(0), ship(1)), (ship(2), ship(3))];
    assert_eq!(desk.locks_wanted().into_iter().collect::<Vec<_>>(), wanted);
    assert_eq!(desk.locks_wanted().into_iter().collect::<Vec<_>>(), wanted);
    // (And a lock is no request to part a pair.)
    assert!(desk.take_requests().is_empty());
    desk.cancel_lock((ship(0), ship(1)));
    assert!(!desk.is_lock_wanted((ship(0), ship(1))));
    assert_eq!(
        desk.locks_wanted().into_iter().collect::<Vec<_>>(),
        vec![(ship(2), ship(3))]
    );
    // A pair that is not wanted locked is cancelled without a word.
    desk.cancel_lock((ship(5), ship(6)));
    assert_eq!(desk.locks_wanted().len(), 1);
}

#[test]
fn asking_to_part_a_pair_takes_the_lock_that_was_wanted_of_it_and_of_no_other() {
    let desk = a_chain();
    desk.lock((ship(0), ship(1)));
    desk.lock((ship(2), ship(3)));
    desk.decouple((ship(0), ship(1)));
    assert!(!desk.is_lock_wanted((ship(0), ship(1))));
    assert!(desk.is_lock_wanted((ship(2), ship(3))));
    // (A lock asked for a pair that is asked to be parted is not wanted at once, whichever came first.)
    desk.lock((ship(0), ship(1)));
    assert!(desk.is_lock_wanted((ship(0), ship(1))));
}

#[test]
fn the_locks_that_are_wanted_of_pairs_that_are_gone_are_let_go_and_the_rest_are_kept() {
    let desk = a_chain();
    desk.lock((ship(0), ship(1)));
    desk.lock((ship(2), ship(3)));
    desk.keep_locks_only(|pair| pair == (ship(2), ship(3)));
    assert_eq!(
        desk.locks_wanted().into_iter().collect::<Vec<_>>(),
        vec![(ship(2), ship(3))]
    );
}

#[test]
fn a_clone_shares_the_locks_wanted_too() {
    let desk = a_chain();
    let other = desk.clone();
    other.lock((ship(1), ship(2)));
    assert!(desk.is_lock_wanted((ship(1), ship(2))));
}

#[test]
fn a_time_to_brake_over_lasts_until_it_is_ended_and_is_not_taken_as_a_request_is() {
    let desk = a_chain();
    desk.immobilize((ship(2), ship(3)), 0.5);
    desk.immobilize((ship(0), ship(1)), 2.0);
    assert!(desk.is_braking((ship(0), ship(1))));
    assert!(!desk.is_braking((ship(1), ship(2))));
    // In order of pair, with the time left; asking does not take them.
    let braking = vec![((ship(0), ship(1)), 2.0), ((ship(2), ship(3)), 0.5)];
    assert_eq!(desk.brakes().into_iter().collect::<Vec<_>>(), braking);
    assert_eq!(desk.brakes().into_iter().collect::<Vec<_>>(), braking);
    // The time left is set as it runs down, and asking again for a pair that is braking says the new time.
    desk.set_brake_left((ship(0), ship(1)), 1.5);
    assert_eq!(desk.brakes()[&(ship(0), ship(1))], 1.5);
    desk.immobilize((ship(0), ship(1)), 4.0);
    assert_eq!(desk.brakes()[&(ship(0), ship(1))], 4.0);
    // A time left cannot be set for a pair that is not braking.
    desk.set_brake_left((ship(1), ship(2)), 3.0);
    assert!(!desk.is_braking((ship(1), ship(2))));
    desk.end_brake((ship(0), ship(1)));
    desk.end_brake((ship(5), ship(6)));
    assert_eq!(
        desk.brakes().into_iter().collect::<Vec<_>>(),
        vec![((ship(2), ship(3)), 0.5)]
    );
    assert!(desk.take_requests().is_empty() && desk.mobilizations().is_empty());
}

#[test]
fn mobilizing_a_pair_takes_back_its_brake_and_the_lock_wanted_of_it_and_waits_the_time_it_is_given()
{
    let desk = a_chain();
    desk.immobilize((ship(0), ship(1)), 1.0);
    desk.lock((ship(0), ship(1)));
    desk.immobilize((ship(2), ship(3)), 1.0);
    desk.mobilize((ship(0), ship(1)), 0.0);
    // Asking again says the new time.
    desk.mobilize((ship(0), ship(1)), 2.5);
    assert!(!desk.is_braking((ship(0), ship(1))));
    assert!(!desk.is_lock_wanted((ship(0), ship(1))));
    assert!(desk.is_braking((ship(2), ship(3))));
    assert!(desk.is_mobilizing((ship(0), ship(1))));
    assert!(!desk.is_mobilizing((ship(2), ship(3))));
    assert_eq!(
        desk.mobilizations().into_iter().collect::<Vec<_>>(),
        vec![((ship(0), ship(1)), 2.5)]
    );
    // The time left is set as it runs down; it cannot be set for a pair that does not wait.
    desk.set_mobilization_left((ship(0), ship(1)), 1.0);
    desk.set_mobilization_left((ship(2), ship(3)), 1.0);
    assert_eq!(
        desk.mobilizations().into_iter().collect::<Vec<_>>(),
        vec![((ship(0), ship(1)), 1.0)]
    );
    desk.end_mobilization((ship(0), ship(1)));
    assert!(desk.mobilizations().is_empty());
}

#[test]
fn a_wait_to_mobilize_is_taken_back_by_immobilizing_or_parting_and_is_let_go_with_a_pair_that_is_gone()
 {
    let desk = a_chain();
    desk.mobilize((ship(0), ship(1)), 3.0);
    desk.mobilize((ship(1), ship(2)), 3.0);
    desk.mobilize((ship(2), ship(3)), 3.0);
    desk.immobilize((ship(0), ship(1)), 1.0);
    desk.decouple((ship(1), ship(2)));
    assert!(!desk.is_mobilizing((ship(0), ship(1))));
    assert!(!desk.is_mobilizing((ship(1), ship(2))));
    assert!(desk.is_mobilizing((ship(2), ship(3))));
    desk.mobilize((ship(0), ship(1)), 3.0);
    desk.keep_mobilizations_only(|pair| pair == (ship(0), ship(1)));
    assert_eq!(
        desk.mobilizations().into_keys().collect::<Vec<_>>(),
        vec![(ship(0), ship(1))]
    );
}

#[test]
fn immobilizing_takes_the_lock_wanted_back_and_parting_takes_the_brake_back() {
    let desk = a_chain();
    desk.lock((ship(0), ship(1)));
    desk.lock((ship(2), ship(3)));
    desk.immobilize((ship(0), ship(1)), 1.0);
    assert!(!desk.is_lock_wanted((ship(0), ship(1))));
    assert!(desk.is_lock_wanted((ship(2), ship(3))));
    desk.immobilize((ship(2), ship(3)), 1.0);
    desk.decouple((ship(0), ship(1)));
    assert!(!desk.is_braking((ship(0), ship(1))));
    assert!(desk.is_braking((ship(2), ship(3))));
}

#[test]
fn the_brakes_of_pairs_that_are_gone_are_let_go_and_the_rest_are_kept() {
    let desk = a_chain();
    desk.immobilize((ship(0), ship(1)), 1.0);
    desk.immobilize((ship(2), ship(3)), 1.0);
    desk.keep_brakes_only(|pair| pair == (ship(2), ship(3)));
    assert_eq!(
        desk.brakes().into_keys().collect::<Vec<_>>(),
        vec![(ship(2), ship(3))]
    );
    let other = desk.clone();
    other.end_brake((ship(2), ship(3)));
    assert!(desk.brakes().is_empty(), "a clone is the same desk");
}

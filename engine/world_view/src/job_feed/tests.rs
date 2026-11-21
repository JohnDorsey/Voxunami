use proptest::prelude::*;

use super::{MAXIMUM_JOBS_PER_WORKER, MINIMUM_JOBS_PER_WORKER, jobs_to_hold};

#[test]
fn threads_that_finished_nothing_are_given_the_least() {
    for workers in [1, 2, 3, 11, 64] {
        assert_eq!(
            jobs_to_hold(workers, 0),
            workers * MINIMUM_JOBS_PER_WORKER,
            "{workers} workers"
        );
    }
}

/// Nodes that take longer than a frame (hills with caves: 94 to 201 ms) are made a few at a time, and a thread that has been handed a lot of them is busy for seconds
/// with work the view cannot take back: the least is held for them as it always was.
#[test]
fn threads_that_finish_a_fraction_of_a_node_a_frame_are_given_the_least() {
    let workers = 11;
    // 43 ms a node and 16.7 ms a frame: 0.39 of a node a thread, so 4 of 11.
    assert_eq!(jobs_to_hold(workers, 4), workers * MINIMUM_JOBS_PER_WORKER);
}

/// Threads that did everything they were handed went idle before the next update: twice as many (and a little more) are held from now on, until what they finish in an update
/// is what is held.
#[test]
fn threads_that_ran_dry_are_given_more_than_twice_what_they_had() {
    for workers in [2, 3, 11] {
        let most = workers * MAXIMUM_JOBS_PER_WORKER;
        let mut held = jobs_to_hold(workers, 0);
        while 2 * held + workers <= most {
            let next = jobs_to_hold(workers, held);
            assert!(
                next > 2 * held,
                "{workers} workers: {next} after {held} all finished"
            );
            held = next;
        }
        // And then the most.
        assert_eq!(jobs_to_hold(workers, held), most, "{workers} workers");
    }
}

/// The measured case (11 threads, 1 ms a node, 60 frames a second): the threads can finish about 180 nodes in a frame, and what is held is more than that.
#[test]
fn threads_that_can_finish_a_frame_are_held_more_than_a_frame_of_work() {
    let held = jobs_to_hold(11, 180);
    assert!(
        (180..=11 * MAXIMUM_JOBS_PER_WORKER).contains(&held),
        "{held}"
    );
}

proptest! {
    #[test]
    fn what_is_held_is_between_the_least_and_the_most_and_covers_what_was_finished(
        workers in 1usize..64,
        finished in 0usize..10_000,
    ) {
        let held = jobs_to_hold(workers, finished);
        prop_assert!(held >= workers * MINIMUM_JOBS_PER_WORKER);
        prop_assert!(held <= workers * MAXIMUM_JOBS_PER_WORKER);
        // (Unless the most is less than what was finished: then it is the most.)
        prop_assert!(held >= finished.min(workers * MAXIMUM_JOBS_PER_WORKER));
    }

    /// Finishing more never makes the view hold less.
    #[test]
    fn finishing_more_never_holds_less(
        workers in 1usize..64,
        finished in 0usize..10_000,
        more in 0usize..10_000,
    ) {
        prop_assert!(jobs_to_hold(workers, finished + more) >= jobs_to_hold(workers, finished));
    }
}

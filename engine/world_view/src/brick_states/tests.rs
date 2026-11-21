use super::*;
use voxel_grid::ChannelStorage;

fn at(x: i128) -> BrickCoordinate {
    BrickCoordinate::new(x, 0, 0)
}

fn voxels() -> Arc<Brick> {
    Arc::new(Brick::from_channels(vec![ChannelStorage::uniform(0)]))
}

/// A brick that was loaded, meshed, and whose picture is waiting.
fn waiting(states: &mut BrickStates, coordinate: BrickCoordinate) {
    states.load(coordinate, voxels());
    states.start_meshing(coordinate);
    assert!(states.finish_meshing(coordinate));
    states.hold(coordinate, PackedBrickMesh::default());
}

fn assert_consistent(states: &BrickStates, held_back: bool) {
    if let Err(problem) = states.check_consistency(held_back) {
        panic!("{problem}");
    }
}

#[test]
fn a_brick_that_nothing_is_known_of_has_no_record() {
    let mut states = BrickStates::new();
    assert!(states.get(at(0)).is_none());
    states.start_generating(at(0));
    assert!(states.get(at(0)).is_some());
    states.forget_data(at(0));
    assert!(
        states.get(at(0)).is_none(),
        "a record that says nothing is dropped"
    );
    assert_eq!(states.jobs_in_flight(), 0);
    assert_consistent(&states, true);
}

#[test]
fn the_counts_follow_the_changes() {
    let mut states = BrickStates::new();
    states.load(at(0), voxels());
    states.start_generating(at(1));
    assert_eq!((states.loaded_count(), states.jobs_in_flight()), (1, 1));

    states.start_meshing(at(0));
    assert_eq!((states.jobs_in_flight(), states.meshed_count()), (2, 0));
    assert!(states.finish_meshing(at(0)));
    states.deliver(at(0));
    assert_eq!((states.jobs_in_flight(), states.meshed_count()), (1, 1));

    states.finish_generating(at(1), voxels());
    assert_eq!((states.loaded_count(), states.jobs_in_flight()), (2, 0));
    states.unload(at(0));
    assert_eq!((states.loaded_count(), states.meshed_count()), (1, 0));
    assert_consistent(&states, false);

    let displayed = states.clear();
    assert!(displayed.is_empty());
    assert_eq!((states.loaded_count(), states.jobs_in_flight()), (0, 0));
}

#[test]
fn voxels_that_were_generated_do_not_replace_the_ones_a_brick_was_loaded_with() {
    let mut states = BrickStates::new();
    let known = voxels();
    states.start_generating(at(0));
    states.load(at(0), Arc::clone(&known));
    assert_eq!(states.jobs_in_flight(), 1, "the job is still running");
    states.finish_generating(at(0), voxels());
    assert_eq!(states.jobs_in_flight(), 0);
    assert!(Arc::ptr_eq(states.data(at(0)).unwrap(), &known));
}

#[test]
fn the_result_of_a_generating_job_that_was_given_up_is_ignored() {
    let mut states = BrickStates::new();
    states.start_generating(at(0));
    states.forget_data(at(0));
    states.finish_generating(at(0), voxels());
    assert!(states.data(at(0)).is_none());
    assert_eq!(states.loaded_count(), 0);
    assert_consistent(&states, true);
}

#[test]
fn a_brick_that_was_given_out_and_taken_down_has_no_picture_and_is_to_be_meshed_again() {
    let mut states = BrickStates::new();
    waiting(&mut states, at(0));
    assert!(states.is_waiting(at(0)));
    assert!(states.hand_out(at(0)).is_some());
    assert!(!states.is_waiting(at(0)), "it was handed out");
    assert!(states.is_shown(at(0)));
    assert_eq!(
        states.hand_out(at(0)),
        None,
        "there is nothing more to give"
    );
    assert_consistent(&states, true);

    assert_eq!(
        states.take_down(at(0)),
        Some(false),
        "nothing was on the screen"
    );
    assert!(!states.is_shown(at(0)));
    assert!(states.get(at(0)).unwrap().needs_meshing());
    assert_eq!(states.meshed_count(), 0);
    assert_eq!(states.take_down(at(0)), None);
    assert_consistent(&states, true);
}

#[test]
fn a_brick_that_is_not_waiting_cannot_be_handed_out_and_one_not_shown_cannot_be_taken_down() {
    let mut states = BrickStates::new();
    assert_eq!(states.hand_out(at(0)), None);
    assert_eq!(states.take_down(at(0)), None);
    assert!(states.get(at(0)).is_none(), "asking made no record");
    states.load(at(0), voxels());
    assert_eq!(states.hand_out(at(0)), None);
    assert_eq!(states.take_down(at(0)), None);
}

#[test]
fn a_job_is_wanted_only_while_its_brick_is_being_meshed() {
    let mut states = BrickStates::new();
    states.load(at(0), voxels());
    states.start_meshing(at(0));
    states.unload(at(0));
    assert!(!states.finish_meshing(at(0)), "it went out of range");

    states.load(at(0), voxels());
    states.start_meshing(at(0));
    states.drop_all_pictures();
    assert!(
        !states.finish_meshing(at(0)),
        "what it is drawn from changed"
    );

    assert!(!states.finish_meshing(at(5)), "a brick nothing is known of");
    assert!(states.get(at(5)).is_none());
}

#[test]
fn a_brick_that_goes_out_of_range_stays_shown_and_on_the_screen_for_the_viewer_to_take_down() {
    let mut states = BrickStates::new();
    waiting(&mut states, at(0));
    states.hand_out(at(0)).unwrap();
    states.set_displayed(at(0), true);
    states.unload(at(0));
    assert_eq!(states.loaded_count(), 0);
    assert!(states.is_shown(at(0)) && states.is_displayed(at(0)));
    assert!(states.get(at(0)).unwrap().needs_meshing());
    assert_consistent(&states, true);
    assert_eq!(states.take_down(at(0)), Some(true));
    assert!(states.get(at(0)).is_none(), "now it says nothing");
}

#[test]
fn an_out_of_date_picture_is_dropped_and_a_job_making_one_is_left_to_be_found_out_of_date() {
    let mut states = BrickStates::new();
    waiting(&mut states, at(0));
    states.load(at(1), voxels());
    states.start_meshing(at(1));
    states.drop_picture(at(0));
    states.drop_picture(at(1));
    assert!(states.get(at(0)).unwrap().needs_meshing());
    assert!(states.get(at(1)).unwrap().is_meshing());
    assert_eq!(states.meshed_count(), 0);
    assert_consistent(&states, true);
}

#[test]
fn clearing_says_what_the_renderer_held_in_order() {
    let mut states = BrickStates::new();
    for x in [3, 1, 2] {
        waiting(&mut states, at(x));
        states.hand_out(at(x)).unwrap();
        states.set_displayed(at(x), true);
    }
    waiting(&mut states, at(0));
    assert_eq!(states.clear(), [at(1), at(2), at(3)]);
    assert!(states.get(at(1)).is_none());
}

#[test]
fn what_the_changes_cannot_prevent_is_found_by_the_check() {
    let mut states = BrickStates::new();
    waiting(&mut states, at(0));
    assert!(states.hand_out(at(0)).is_some());
    states.records.get_mut(&at(0)).unwrap().shown = false;
    let problem = states.check_consistency(true).unwrap_err();
    assert!(problem.contains("would never be meshed again"), "{problem}");

    let mut states = BrickStates::new();
    waiting(&mut states, at(0));
    states.tally.loaded += 1;
    let problem = states.check_consistency(true).unwrap_err();
    assert!(problem.contains("tally"), "{problem}");

    let mut states = BrickStates::new();
    waiting(&mut states, at(0));
    states.records.get_mut(&at(0)).unwrap().data = None;
    let problem = states.check_consistency(true).unwrap_err();
    assert!(problem.contains("not loaded"), "{problem}");
}

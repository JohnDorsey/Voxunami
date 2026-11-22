use super::*;
use voxel_grid::ChannelStorage;
use voxel_meshing::{OCCUPANCY_WORDS, PackedQuad};

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
    states.deliver(at(0), 0);
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

// Held pictures: a brick that is taken down keeps its picture (the renderer holds the mesh, hidden), so that showing it again is a move.

/// A picture of `quads` quads, which is empty for none.
fn mesh_of(quads: usize) -> PackedBrickMesh {
    PackedBrickMesh {
        quads: vec![
            PackedQuad {
                geometry: 1,
                material: 1,
            };
            quads
        ],
        detailed: Vec::new(),
    }
}

/// A brick that is shown as the mesher shows one: loaded, meshed, handed out, and on the screen if it has something to see.
fn on_screen(states: &mut BrickStates, coordinate: BrickCoordinate, mesh: PackedBrickMesh) {
    states.load(coordinate, voxels());
    states.start_meshing(coordinate);
    assert!(states.finish_meshing(coordinate));
    states.hold(coordinate, mesh);
    let mesh = states.hand_out(coordinate).expect("it was waiting");
    states.set_displayed(coordinate, !mesh.is_empty());
}

/// A brick that was shown and is held, with this picture.
fn held(states: &mut BrickStates, coordinate: BrickCoordinate, mesh: PackedBrickMesh) {
    on_screen(states, coordinate, mesh);
    assert!(matches!(
        states.hold_down(coordinate),
        Some(Taken::Hidden | Taken::Nothing)
    ));
}

fn a_mixed_occupancy() -> BrickOccupancy {
    let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
    words[0] = 1;
    BrickOccupancy::from_words(words)
}

#[test]
fn without_a_budget_a_brick_taken_down_is_dropped_as_it_always_was() {
    let mut states = BrickStates::new();
    on_screen(&mut states, at(0), mesh_of(100));
    assert_eq!(states.hold_down(at(0)), Some(Taken::Removed));
    assert!(!states.is_shown(at(0)) && !states.is_held(at(0)));
    assert!(!states.is_displayed(at(0)));
    assert!(states.get(at(0)).unwrap().needs_meshing());
    assert_eq!(states.held_pictures(), HeldBricks::default());
    assert_consistent(&states, true);

    // A brick that is not shown cannot be taken down, held or not.
    assert_eq!(states.hold_down(at(0)), None);
    assert_eq!(states.hold_down(at(9)), None);
}

#[test]
fn with_a_budget_a_brick_taken_down_is_held_hidden_and_is_not_made_again() {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    let mesh = mesh_of(100);
    let bytes = mesh.byte_size();
    assert!(bytes > MINIMUM_HELD_COST);
    on_screen(&mut states, at(0), mesh);
    assert_eq!(states.hold_down(at(0)), Some(Taken::Hidden));
    assert!(!states.is_shown(at(0)));
    assert!(states.is_held(at(0)) && states.is_ready(at(0)));
    assert!(
        !states.is_waiting(at(0)) && states.hand_out(at(0)).is_none(),
        "there is no picture to hand out: the renderer has it"
    );
    assert!(!states.get(at(0)).unwrap().needs_meshing());
    assert!(
        states.is_displayed(at(0)),
        "the renderer still holds the mesh"
    );
    assert_eq!(
        states.held_pictures(),
        HeldBricks {
            bricks: 1,
            with_mesh: 1,
            bytes
        }
    );
    assert_eq!(states.meshed_count(), 0, "it is not one that was given out");
    assert_consistent(&states, true);
}

#[test]
fn a_brick_with_nothing_to_see_is_held_without_a_mesh_and_charged_a_little() {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    on_screen(&mut states, at(0), mesh_of(0));
    assert_eq!(states.hold_down(at(0)), Some(Taken::Nothing));
    assert!(states.is_held(at(0)) && !states.is_displayed(at(0)));
    assert_eq!(
        states.held_pictures(),
        HeldBricks {
            bricks: 1,
            with_mesh: 0,
            bytes: MINIMUM_HELD_COST
        }
    );
    assert_consistent(&states, true);
}

#[test]
fn a_held_brick_is_shown_again_by_a_move_and_is_held_again_when_taken_down_again() {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    let mesh = mesh_of(100);
    let bytes = mesh.byte_size();
    held(&mut states, at(0), mesh);
    held(&mut states, at(1), mesh_of(0));

    assert_eq!(
        states.reveal(at(0)),
        Some(true),
        "the renderer is to show it"
    );
    assert_eq!(
        states.reveal(at(1)),
        Some(false),
        "there is nothing on the renderer to show"
    );
    assert!(states.is_shown(at(0)) && states.is_displayed(at(0)));
    assert!(states.is_shown(at(1)) && !states.is_displayed(at(1)));
    assert!(!states.is_held(at(0)));
    assert_eq!(states.held_pictures(), HeldBricks::default());
    assert_consistent(&states, true);
    // Nothing is held now to be shown.
    assert_eq!(states.reveal(at(0)), None);
    assert_eq!(states.reveal(at(7)), None);

    assert_eq!(states.hold_down(at(0)), Some(Taken::Hidden));
    assert_eq!(states.hold_down(at(1)), Some(Taken::Nothing));
    assert_eq!(
        states.held_pictures(),
        HeldBricks {
            bricks: 2,
            with_mesh: 1,
            bytes: bytes + MINIMUM_HELD_COST
        }
    );
    assert_consistent(&states, true);
}

#[test]
fn a_held_picture_stays_when_the_voxels_are_let_go_of_and_can_be_shown_without_them() {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    held(&mut states, at(0), mesh_of(100));
    states.unload(at(0));
    assert_eq!(states.loaded_count(), 0);
    assert!(states.is_held(at(0)) && states.is_ready(at(0)));
    assert!(!states.get(at(0)).unwrap().needs_meshing());
    assert_consistent(&states, true);

    // It is shown before its voxels are loaded again (that is what holding is for), and is not meshed when they are.
    assert_eq!(states.reveal(at(0)), Some(true));
    assert_consistent(&states, true);
    states.load(at(0), voxels());
    assert!(!states.get(at(0)).unwrap().needs_meshing());
    assert_consistent(&states, true);
}

#[test]
fn with_a_budget_a_brick_that_is_shown_when_it_goes_out_of_range_keeps_its_picture_to_be_held_when_it_is_taken_down()
 {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    on_screen(&mut states, at(0), mesh_of(100));
    states.unload(at(0));
    assert_eq!(states.loaded_count(), 0);
    assert!(!states.get(at(0)).unwrap().needs_meshing());
    assert_eq!(states.hold_down(at(0)), Some(Taken::Hidden));
    assert!(states.is_held(at(0)));
    assert_consistent(&states, true);

    // Without a budget it is as it was (the test above): the picture goes with the voxels, and is made again if the brick is wanted again.
    let mut states = BrickStates::new();
    on_screen(&mut states, at(0), mesh_of(100));
    states.unload(at(0));
    assert!(states.get(at(0)).unwrap().needs_meshing());
    assert_eq!(states.hold_down(at(0)), Some(Taken::Removed));
}

#[test]
fn an_edit_lets_go_of_a_held_picture_and_says_whether_the_renderer_is_to_take_its_mesh_down() {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    held(&mut states, at(0), mesh_of(100));
    held(&mut states, at(1), mesh_of(0));
    on_screen(&mut states, at(2), mesh_of(100));

    assert!(
        states.drop_picture(at(0)),
        "its hidden mesh is to be taken down"
    );
    assert!(!states.is_displayed(at(0)));
    assert!(states.get(at(0)).unwrap().needs_meshing());
    assert!(!states.drop_picture(at(1)), "it had no mesh");
    assert!(
        !states.drop_picture(at(2)),
        "a brick that is shown keeps its mesh until the new one is made"
    );
    assert!(states.is_displayed(at(2)));
    assert_eq!(states.held_pictures(), HeldBricks::default());
    assert_consistent(&states, true);
}

#[test]
fn every_picture_is_dropped_and_the_renderer_is_told_the_hidden_meshes_to_take_down_in_order() {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    for x in [3, 1, 2] {
        held(&mut states, at(x), mesh_of(100));
    }
    held(&mut states, at(4), mesh_of(0));
    on_screen(&mut states, at(5), mesh_of(100));
    assert_eq!(states.drop_all_pictures(), [at(1), at(2), at(3)]);
    assert_eq!(states.held_pictures(), HeldBricks::default());
    assert!(states.is_shown(at(5)) && states.is_displayed(at(5)));
    assert!(states.get(at(1)).unwrap().needs_meshing());
    assert_consistent(&states, true);
}

#[test]
fn over_the_budget_the_oldest_are_let_go_of_first_and_never_one_that_is_kept() {
    let mut states = BrickStates::new();
    let bytes = mesh_of(100).byte_size();
    states.set_budget(2 * bytes + 10);
    for x in 1..=4 {
        held(&mut states, at(x), mesh_of(100));
    }
    assert_eq!(states.held_pictures().bytes, 4 * bytes);
    assert_eq!(states.evict_over_budget(|_| false), [at(1), at(2)]);
    assert_eq!(
        states.held_pictures(),
        HeldBricks {
            bricks: 2,
            with_mesh: 2,
            bytes: 2 * bytes
        }
    );
    assert!(!states.is_displayed(at(1)));
    assert!(states.get(at(1)).unwrap().needs_meshing());
    assert!(
        states.evict_over_budget(|_| false).is_empty(),
        "within the budget now"
    );
    assert_consistent(&states, true);

    // The older of the two is kept (it is wanted), so the other goes, and what is held is over the budget by the one that is kept.
    states.set_budget(1);
    assert_eq!(
        states.evict_over_budget(|coordinate| coordinate == at(3)),
        [at(4)]
    );
    assert_eq!(
        states.held_pictures(),
        HeldBricks {
            bricks: 1,
            with_mesh: 1,
            bytes
        }
    );
    assert!(states.is_held(at(3)));
    assert_consistent(&states, true);
}

#[test]
fn a_held_brick_with_nothing_to_see_is_let_go_of_without_telling_the_renderer_and_its_record_goes_when_it_says_nothing()
 {
    let mut states = BrickStates::new();
    states.set_budget(MINIMUM_HELD_COST);
    held(&mut states, at(1), mesh_of(0));
    held(&mut states, at(2), mesh_of(0));
    states.unload(at(1));
    assert_eq!(
        states.evict_over_budget(|_| false),
        Vec::<BrickCoordinate>::new()
    );
    assert_eq!(states.held_pictures().bricks, 1);
    assert!(
        states.get(at(1)).is_none(),
        "an unloaded brick with no picture says nothing"
    );
    assert!(states.get(at(2)).is_some());
    assert_consistent(&states, true);
}

#[test]
fn clearing_says_what_the_renderer_held_the_hidden_meshes_of_held_pictures_too_and_forgets_them() {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    for x in [3, 1] {
        on_screen(&mut states, at(x), mesh_of(100));
    }
    held(&mut states, at(2), mesh_of(100));
    held(&mut states, at(4), mesh_of(0));
    assert_eq!(states.clear(), [at(1), at(2), at(3)]);
    assert_eq!(states.held_pictures(), HeldBricks::default());
    assert!(states.get(at(2)).is_none());
}

#[test]
fn a_brick_whose_picture_is_being_made_again_is_not_held_when_it_is_taken_down() {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    on_screen(&mut states, at(0), mesh_of(100));
    // An edit: its picture is out of date, the old mesh stays on the screen, and a job is making the new one.
    states.drop_picture(at(0));
    states.start_meshing(at(0));
    assert_eq!(states.hold_down(at(0)), Some(Taken::Removed));
    assert!(!states.is_held(at(0)) && !states.is_shown(at(0)));
    assert_eq!(states.held_pictures(), HeldBricks::default());
    // The job's picture is up to date, and waits to be shown as it did before holding.
    assert!(states.finish_meshing(at(0)));
    states.hold(at(0), mesh_of(3));
    assert!(states.is_waiting(at(0)) && states.is_ready(at(0)));
    assert_consistent(&states, true);
}

#[test]
fn the_occupancy_of_a_brick_is_kept_with_its_picture_for_as_long_as_the_picture_is_and_is_charged_to_it()
 {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    let mesh = mesh_of(100);
    let bytes = mesh.byte_size();
    let occupancy = a_mixed_occupancy();
    on_screen(&mut states, at(0), mesh);
    states.keep_occupancy(at(0), occupancy.clone());
    assert_eq!(states.occupancy(at(0)), Some(&occupancy));

    assert_eq!(states.hold_down(at(0)), Some(Taken::Hidden));
    states.unload(at(0));
    assert_eq!(
        states.occupancy(at(0)),
        Some(&occupancy),
        "it is what is told again when the brick is shown"
    );
    assert_eq!(
        states.held_pictures().bytes,
        bytes + occupancy.byte_size(),
        "the occupancy is held too, and charged"
    );
    assert_eq!(states.reveal(at(0)), Some(true));
    assert_eq!(
        states.occupancy(at(0)),
        Some(&occupancy),
        "it is held again if the brick is taken down again"
    );
    assert_eq!(states.hold_down(at(0)), Some(Taken::Hidden));

    // The picture goes, the occupancy with it.
    assert!(states.drop_picture(at(0)));
    assert_eq!(states.occupancy(at(0)), None);
    assert_consistent(&states, true);
}

#[test]
fn what_the_check_says_of_held_pictures() {
    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    held(&mut states, at(0), mesh_of(100));
    assert_consistent(&states, true);
    states.records.get_mut(&at(0)).unwrap().shown = true;
    let problem = states.check_consistency(true).unwrap_err();
    assert!(
        problem.contains("held") && problem.contains("shown"),
        "{problem}"
    );

    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    held(&mut states, at(0), mesh_of(100));
    states.held_order.clear();
    let problem = states.check_consistency(true).unwrap_err();
    assert!(problem.contains("order"), "{problem}");

    let mut states = BrickStates::new();
    states.set_budget(1 << 20);
    held(&mut states, at(0), mesh_of(100));
    states.tally.held_bytes += 1;
    let problem = states.check_consistency(true).unwrap_err();
    assert!(problem.contains("tally"), "{problem}");

    // A brick is on the screen and not shown only if its picture is held.
    let mut states = BrickStates::new();
    on_screen(&mut states, at(0), mesh_of(100));
    states.records.get_mut(&at(0)).unwrap().shown = false;
    assert!(states.check_consistency(true).is_err());
}

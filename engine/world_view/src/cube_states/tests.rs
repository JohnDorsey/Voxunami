use voxel_meshing::PackedQuad;
use voxel_space::BrickCoordinate;

use super::*;

fn node(x: i128) -> BrickCube {
    BrickCube::new(BrickCoordinate::new(2 * x, 0, 0), 1).unwrap()
}

fn brick(x: i128) -> BrickCube {
    BrickCube::new(BrickCoordinate::new(x, 0, 0), 0).unwrap()
}

/// A picture with something to see.
fn picture() -> PackedBrickMesh {
    PackedBrickMesh {
        quads: vec![PackedQuad {
            geometry: 1,
            material: 1,
        }],
        detailed: Vec::new(),
    }
}

fn assert_consistent(states: &CubeStates) {
    if let Err(problem) = states.check_consistency() {
        panic!("{problem}");
    }
}

/// A node that is wanted, made from data of version 0, and waiting.
fn waiting(states: &mut CubeStates, cube: BrickCube, mesh: PackedBrickMesh) {
    if !states.wanted.contains(&cube) {
        let mut wanted: Vec<BrickCube> = states.wanted.iter().copied().collect();
        wanted.push(cube);
        states.want(&wanted);
    }
    states.start_making(cube, 0);
    assert_eq!(states.finish_making(cube, 0, 0, mesh), Arrival::Waiting);
}

#[test]
fn wanting_makes_records_for_nodes_and_not_for_bricks_and_a_node_nobody_wants_has_none() {
    let mut states = CubeStates::new();
    states.want(&[brick(0), node(1), node(2)]);
    assert!(states.has_node(node(1)) && states.has_node(node(2)));
    assert!(!states.has_node(brick(0)));
    assert_eq!(states.wanted().len(), 3);
    assert_consistent(&states);

    states.want(&[node(2)]);
    assert!(!states.has_node(node(1)), "neither wanted nor shown");
    assert!(states.has_node(node(2)));
    assert_consistent(&states);
}

#[test]
fn a_node_that_is_shown_keeps_its_record_when_it_is_no_longer_wanted_until_it_is_taken_down() {
    let mut states = CubeStates::new();
    waiting(&mut states, node(1), picture());
    states.show_node(node(1));
    states.want(&[node(2)]);
    assert!(states.has_node(node(1)), "it is on the screen");
    assert_consistent(&states);

    assert!(states.hide_node(node(1)));
    assert!(!states.has_node(node(1)), "and now it says nothing");
    assert!(!states.shown().contains(&node(1)));
    assert_consistent(&states);
}

#[test]
fn a_picture_is_made_when_a_node_is_wanted_and_has_none_of_this_version_and_none_is_being_made() {
    let mut states = CubeStates::new();
    assert!(
        !states.needs_making(node(1), 0),
        "nothing is known of it, so it is not made"
    );
    states.want(&[node(1)]);
    assert!(states.needs_making(node(1), 0));
    states.start_making(node(1), 0);
    assert!(!states.needs_making(node(1), 0), "it is being made");
    assert!(
        states.needs_making(node(1), 1),
        "from data that has changed"
    );

    assert_eq!(
        states.finish_making(node(1), 0, 0, picture()),
        Arrival::Waiting
    );
    assert!(!states.needs_making(node(1), 0), "it is waiting");
    assert!(states.needs_making(node(1), 1));
    assert!(states.is_ready(node(1), 0));
    assert!(!states.is_ready(node(1), 1));
}

#[test]
fn a_node_that_is_not_wanted_is_not_made_but_one_that_is_shown_is_made_again_when_its_data_changes()
{
    let mut states = CubeStates::new();
    waiting(&mut states, node(1), picture());
    states.show_node(node(1));
    states.want(&[]);
    assert!(
        !states.needs_making(node(1), 0),
        "the picture on the screen is of this version"
    );
    assert!(
        states.needs_making(node(1), 1),
        "it is shown, wanted or not, and out of date"
    );
}

#[test]
fn the_result_of_a_job_whose_data_has_changed_or_whose_node_is_gone_is_thrown_away() {
    let mut states = CubeStates::new();
    states.want(&[node(1)]);
    states.start_making(node(1), 0);
    assert_eq!(
        states.finish_making(node(1), 0, 1, picture()),
        Arrival::Ignored
    );
    assert!(!states.node(node(1)).unwrap().is_requested());
    assert!(!states.is_ready(node(1), 0));
    assert!(!states.is_ready(node(1), 1));

    assert_eq!(
        states.finish_making(node(7), 0, 0, picture()),
        Arrival::Ignored,
        "a node nobody knows"
    );
    assert!(!states.has_node(node(7)));
}

#[test]
fn a_job_of_older_data_does_not_clear_the_request_of_newer_data() {
    let mut states = CubeStates::new();
    states.want(&[node(1)]);
    states.start_making(node(1), 0);
    states.start_making(node(1), 1);
    assert_eq!(
        states.finish_making(node(1), 0, 1, picture()),
        Arrival::Ignored
    );
    assert!(states.node(node(1)).unwrap().is_requested());
}

#[test]
fn a_node_that_is_shown_has_its_picture_replaced_where_it_is() {
    let mut states = CubeStates::new();
    waiting(&mut states, node(1), picture());
    assert_eq!(states.show_node(node(1)), picture());
    assert!(states.shown().contains(&node(1)));
    assert!(!states.is_ready(node(1), 0), "it was handed out");
    assert_consistent(&states);

    assert_eq!(
        states.finish_making(node(1), 1, 1, picture()),
        Arrival::Replaced(Replacement::Upload(picture()))
    );
    assert_eq!(
        states.finish_making(node(1), 2, 2, PackedBrickMesh::default()),
        Arrival::Replaced(Replacement::Remove),
        "it had a mesh and has nothing to see now"
    );
    assert_eq!(
        states.finish_making(node(1), 3, 3, PackedBrickMesh::default()),
        Arrival::Replaced(Replacement::Unchanged)
    );
    assert!(states.uploaded_nodes().is_empty());
    assert_consistent(&states);
}

#[test]
fn a_node_with_nothing_to_see_is_shown_without_a_mesh_and_taken_down_without_one() {
    let mut states = CubeStates::new();
    waiting(&mut states, node(1), PackedBrickMesh::default());
    assert!(states.show_node(node(1)).is_empty());
    assert!(states.shown().contains(&node(1)));
    assert!(states.uploaded_nodes().is_empty());
    assert!(!states.hide_node(node(1)), "the renderer had nothing of it");
}

#[test]
fn a_node_that_was_shown_and_taken_down_while_wanted_has_no_picture_and_is_to_be_made_again() {
    let mut states = CubeStates::new();
    waiting(&mut states, node(1), picture());
    states.show_node(node(1));
    assert_eq!(states.uploaded_nodes(), [node(1)]);
    assert!(states.hide_node(node(1)));
    assert!(states.has_node(node(1)), "it is still wanted");
    assert!(!states.is_ready(node(1), 0));
    assert!(states.needs_making(node(1), 0));
    assert!(!states.shown().contains(&node(1)));
    assert_consistent(&states);
}

#[test]
fn bricks_are_shown_and_taken_down_and_have_no_record() {
    let mut states = CubeStates::new();
    states.want(&[brick(0)]);
    states.show_brick(brick(0));
    assert!(states.shown().contains(&brick(0)));
    assert!(!states.has_node(brick(0)));
    assert_consistent(&states);
    states.hide_brick(brick(0));
    assert!(states.shown().is_empty());
}

#[test]
fn clearing_forgets_every_cube() {
    let mut states = CubeStates::new();
    waiting(&mut states, node(1), picture());
    states.show_node(node(1));
    states.show_brick(brick(5));
    states.clear();
    assert!(states.wanted().is_empty() && states.shown().is_empty());
    assert!(!states.has_node(node(1)));
    assert_eq!(states.requested_count(), 0);
}

#[test]
fn what_the_changes_cannot_prevent_is_found_by_the_check() {
    // A shown node whose record says nothing is on the screen.
    let mut states = CubeStates::new();
    waiting(&mut states, node(1), picture());
    states.show_node(node(1));
    states.nodes.get_mut(&node(1)).unwrap().picture = Picture::Missing;
    let problem = states.check_consistency().unwrap_err();
    assert!(problem.contains("has a picture on the screen"), "{problem}");

    // A record of a node that nobody wants or shows.
    let mut states = CubeStates::new();
    states.nodes.insert(node(1), Node::new());
    let problem = states.check_consistency().unwrap_err();
    assert!(problem.contains("neither wanted nor shown"), "{problem}");

    // A node that is wanted and has no record.
    let mut states = CubeStates::new();
    states.want(&[node(1)]);
    states.nodes.clear();
    let problem = states.check_consistency().unwrap_err();
    assert!(problem.contains("is wanted and has no record"), "{problem}");

    // A node on the screen that is not in the set of what is shown.
    let mut states = CubeStates::new();
    waiting(&mut states, node(1), picture());
    states.show_node(node(1));
    states.shown.clear();
    let problem = states.check_consistency().unwrap_err();
    assert!(problem.contains("has a picture on the screen"), "{problem}");
}

#[test]
fn a_step_on_the_way_has_a_record_is_made_and_goes_when_it_is_not_one_any_more() {
    let mut states = CubeStates::new();
    states.want_with_steps(&[node(1)], &[node(2)]);
    // A step has a record and is to be made, though it is not wanted.
    assert!(!states.wanted().contains(&node(2)));
    assert!(states.has_node(node(2)));
    assert!(states.needs_making(node(2), 0));
    assert_consistent(&states);
    // Said again without it, and not shown: the record goes.
    states.want_with_steps(&[node(1)], &[]);
    assert!(!states.has_node(node(2)));
    assert!(!states.needs_making(node(2), 0));
    assert_consistent(&states);
    // One that is shown keeps its record when the steps change, and loses it when it is taken down.
    states.want_with_steps(&[node(1)], &[node(2)]);
    states.start_making(node(2), 0);
    states.finish_making(node(2), 0, 0, picture());
    states.show_node(node(2));
    states.want_with_steps(&[node(1)], &[]);
    assert!(states.has_node(node(2)));
    assert!(states.hide_node(node(2)) || !states.has_node(node(2)));
    assert!(!states.has_node(node(2)));
    assert_consistent(&states);
}

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

/// A picture of `quads` quads: `quads * 8` bytes.
fn picture_of(quads: usize) -> PackedBrickMesh {
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

/// A node shown with the picture made of data of version 0, with `states` holding up to `budget` bytes of pictures that leave the screen.
fn shown_with_budget(
    states: &mut CubeStates,
    cube: BrickCube,
    mesh: PackedBrickMesh,
    budget: usize,
) {
    states.set_budget(budget);
    waiting(states, cube, mesh);
    states.show_node(cube);
}

#[test]
fn without_a_budget_a_node_taken_down_is_dropped_as_it_always_was() {
    let mut states = CubeStates::new();
    waiting(&mut states, node(1), picture());
    states.show_node(node(1));
    states.want(&[]);
    assert_eq!(states.hold_node(node(1), 0), Taken::Removed);
    assert!(!states.has_node(node(1)));
    assert_eq!(states.held_pictures(), HeldPictures::default());
    assert_consistent(&states);
}

#[test]
fn a_node_taken_down_with_a_budget_is_held_and_ready_and_a_reveal_puts_it_back_without_a_new_picture()
 {
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), picture_of(100), 1 << 20);
    states.want(&[]);
    assert_eq!(states.hold_node(node(1), 0), Taken::Hidden);
    assert!(states.has_node(node(1)), "its picture is kept");
    assert!(!states.shown().contains(&node(1)));
    assert_eq!(
        states.held_pictures(),
        HeldPictures {
            nodes: 1,
            with_mesh: 1,
            bytes: 800
        }
    );
    assert_eq!(
        states.uploaded_nodes(),
        [node(1)],
        "the renderer has a mesh of it"
    );
    assert!(states.is_ready(node(1), 0), "it can be shown at once");
    assert!(states.is_held(node(1)));
    states.want(&[node(1)]);
    assert!(
        !states.needs_making(node(1), 0),
        "nothing is made for a picture that is held"
    );
    assert_consistent(&states);

    assert!(states.reveal_node(node(1)), "the renderer is to show it");
    assert!(states.shown().contains(&node(1)));
    assert_eq!(states.held_pictures(), HeldPictures::default());
    assert_eq!(states.uploaded_nodes(), [node(1)]);
    assert!(
        !states.is_ready(node(1), 0),
        "it is on the screen, not waiting"
    );
    assert_consistent(&states);
}

#[test]
fn a_picture_with_nothing_to_see_is_held_without_a_mesh_and_costs_a_little_so_that_the_records_are_bounded()
 {
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), PackedBrickMesh::default(), 1 << 20);
    states.want(&[]);
    assert_eq!(states.hold_node(node(1), 0), Taken::Nothing);
    assert_eq!(
        states.held_pictures(),
        HeldPictures {
            nodes: 1,
            with_mesh: 0,
            bytes: MINIMUM_HELD_COST
        }
    );
    assert!(states.uploaded_nodes().is_empty());
    assert!(
        states.is_ready(node(1), 0),
        "known to be empty: not made again"
    );
    assert!(
        !states.reveal_node(node(1)),
        "the renderer has nothing to show"
    );
    assert_consistent(&states);
}

#[test]
fn a_node_with_a_job_in_flight_or_of_older_data_is_not_held_for_what_would_come_of_it_is_unclear() {
    // Out of date: the data is version 1 now and the picture is of 0.
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), picture(), 1 << 20);
    states.want(&[]);
    assert_eq!(states.hold_node(node(1), 1), Taken::Removed);
    assert!(!states.has_node(node(1)));

    // A job is making it again.
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), picture(), 1 << 20);
    states.start_making(node(1), 0);
    states.want(&[]);
    assert_eq!(states.hold_node(node(1), 0), Taken::Removed);
    assert_eq!(states.held_pictures(), HeldPictures::default());
    assert_consistent(&states);
}

#[test]
fn held_pictures_over_the_budget_go_oldest_first_and_never_the_ones_that_are_wanted() {
    let mut states = CubeStates::new();
    // Four pictures of 800 bytes, held in the order 1, 2, 3, 4; room for two.
    for x in 1..=4 {
        shown_with_budget(&mut states, node(x), picture_of(100), 1600);
    }
    states.want(&[]);
    for x in 1..=4 {
        assert_eq!(states.hold_node(node(x), 0), Taken::Hidden);
    }
    assert_eq!(states.held_pictures().bytes, 3200);
    // The oldest, 1, is wanted again: it is pinned, so 2 and 3 go and 1 stays.
    states.want(&[node(1)]);
    assert_eq!(states.evict_over_budget(), [node(2), node(3)]);
    assert!(states.is_held(node(1)) && states.is_held(node(4)));
    assert!(!states.has_node(node(2)) && !states.has_node(node(3)));
    assert_eq!(
        states.held_pictures(),
        HeldPictures {
            nodes: 2,
            with_mesh: 2,
            bytes: 1600
        }
    );
    assert!(
        states.evict_over_budget().is_empty(),
        "within the budget: nothing more"
    );
    assert_consistent(&states);

    // Everything over the budget is wanted: the budget is passed by what is pinned, and nothing is taken.
    states.want(&[node(1), node(4)]);
    states.set_budget(0);
    assert!(states.evict_over_budget().is_empty());
    assert_eq!(states.held_pictures().nodes, 2);
    // Not wanted any more: they go, the oldest first.
    states.want(&[]);
    assert_eq!(states.evict_over_budget(), [node(1), node(4)]);
    assert_eq!(states.held_pictures(), HeldPictures::default());
    assert_consistent(&states);
}

#[test]
fn a_picture_of_nothing_that_is_evicted_says_nothing_to_the_renderer() {
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), PackedBrickMesh::default(), 1 << 20);
    states.want(&[]);
    states.hold_node(node(1), 0);
    states.set_budget(0);
    assert!(
        states.evict_over_budget().is_empty(),
        "the renderer holds nothing of it"
    );
    assert!(!states.has_node(node(1)), "and the record is gone");
}

#[test]
fn a_held_picture_of_older_data_waits_until_it_is_wanted_and_is_then_dropped_and_made_again() {
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), picture(), 1 << 20);
    states.want(&[]);
    states.hold_node(node(1), 0);
    // The data has changed (version 1), and nobody wants the node: nothing is done.
    assert!(!states.needs_making(node(1), 1));
    assert!(
        !states.is_ready(node(1), 1),
        "the picture is of the old data"
    );
    // Wanted: it is to be made, and the picture the renderer holds hidden goes first.
    states.want(&[node(1)]);
    assert!(states.needs_making(node(1), 1));
    assert_eq!(states.drop_stale_held(node(1), 1), Taken::Removed);
    assert!(!states.is_held(node(1)));
    assert!(states.has_node(node(1)), "wanted, so it keeps a record");
    assert!(states.needs_making(node(1), 1));
    assert_eq!(states.held_pictures(), HeldPictures::default());
    assert_consistent(&states);
    // A picture of the data now is not dropped.
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), picture(), 1 << 20);
    states.want(&[node(1)]);
    states.hold_node(node(1), 0);
    assert_eq!(states.drop_stale_held(node(1), 0), Taken::Nothing);
    assert!(states.is_held(node(1)));
}

#[test]
fn clearing_forgets_held_pictures_and_the_renderer_is_to_take_down_their_meshes() {
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), picture(), 1 << 20);
    shown_with_budget(&mut states, node(2), picture(), 1 << 20);
    states.want(&[]);
    states.hold_node(node(1), 0);
    let mut uploaded = states.uploaded_nodes();
    uploaded.sort_by_key(|cube| cube.origin().x());
    assert_eq!(uploaded, [node(1), node(2)], "held and on the screen");
    states.clear();
    assert_eq!(states.held_pictures(), HeldPictures::default());
    assert!(states.uploaded_nodes().is_empty());
    assert!(states.evict_over_budget().is_empty());
    assert_consistent(&states);
}

#[test]
fn what_the_check_says_of_held_pictures() {
    // Held and on the screen at once.
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), picture(), 1 << 20);
    states.want(&[]);
    states.hold_node(node(1), 0);
    states.shown.insert(node(1));
    let problem = states.check_consistency().unwrap_err();
    assert!(problem.contains("is held and shown"), "{problem}");

    // Held with a job in flight.
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), picture(), 1 << 20);
    states.want(&[]);
    states.hold_node(node(1), 0);
    states.nodes.get_mut(&node(1)).unwrap().requested = Some(0);
    let problem = states.check_consistency().unwrap_err();
    assert!(
        problem.contains("is held and a job is making it"),
        "{problem}"
    );

    // The total is not the sum.
    let mut states = CubeStates::new();
    shown_with_budget(&mut states, node(1), picture(), 1 << 20);
    states.want(&[]);
    states.hold_node(node(1), 0);
    states.held_bytes += 1;
    let problem = states.check_consistency().unwrap_err();
    assert!(problem.contains("held pictures"), "{problem}");
}

/// A node of the warm cut that has been made: it is pinned, not wanted, and its picture is waiting.
fn made_for_the_warm_cut(states: &mut CubeStates, cube: BrickCube, mesh: PackedBrickMesh) {
    let mut pinned: Vec<BrickCube> = states.pinned.iter().copied().collect();
    pinned.push(cube);
    states.pin(&pinned);
    assert!(states.needs_making(cube, 0));
    states.start_making(cube, 0);
    assert_eq!(states.finish_making(cube, 0, 0, mesh), Arrival::Waiting);
}

#[test]
fn a_pinned_node_has_a_record_and_is_made_though_nothing_wants_it_and_an_unpinned_one_that_was_never_made_has_none()
 {
    let mut states = CubeStates::new();
    states.pin(&[brick(0), node(1), node(2)]);
    assert!(states.has_node(node(1)) && states.has_node(node(2)));
    assert!(!states.has_node(brick(0)), "a brick is the mesher's");
    assert!(states.needs_making(node(1), 0));
    assert!(states.wanted().is_empty(), "nothing is to be drawn");
    assert_consistent(&states);

    states.pin(&[node(2)]);
    assert!(
        !states.has_node(node(1)),
        "no longer pinned, and never made"
    );
    assert!(states.has_node(node(2)));
    states.pin(&[]);
    assert!(!states.has_node(node(2)));
    assert_consistent(&states);
}

#[test]
fn a_picture_of_the_warm_cut_that_is_made_is_parked_as_a_held_one_the_renderer_is_given_hidden() {
    let mut states = CubeStates::new();
    states.set_budget(1 << 20);
    made_for_the_warm_cut(&mut states, node(1), picture_of(100));
    let mesh = states.park(node(1)).expect("it is waiting and pinned");
    assert_eq!(mesh, picture_of(100));
    assert!(states.is_held(node(1)) && !states.shown().contains(&node(1)));
    assert!(states.is_ready(node(1), 0));
    assert_eq!(
        states.held_pictures(),
        HeldPictures {
            nodes: 1,
            with_mesh: 1,
            bytes: 800
        }
    );
    assert!(states.park(node(1)).is_none(), "once");
    assert_eq!(states.uploaded_nodes(), [node(1)]);
    assert_consistent(&states);
}

#[test]
fn a_picture_of_nothing_in_the_warm_cut_is_held_without_a_mesh_in_the_renderer() {
    let mut states = CubeStates::new();
    states.set_budget(1 << 20);
    made_for_the_warm_cut(&mut states, node(1), PackedBrickMesh::default());
    let mesh = states.park(node(1)).expect("parked");
    assert!(mesh.is_empty(), "nothing is to be uploaded");
    assert_eq!(states.held_pictures().with_mesh, 0);
    assert!(states.uploaded_nodes().is_empty());
    assert!(states.is_ready(node(1), 0), "and it is not made again");
    assert_consistent(&states);
}

#[test]
fn a_picture_that_is_wanted_is_not_parked_for_the_planner_shows_it_and_without_a_budget_none_is() {
    let mut states = CubeStates::new();
    states.set_budget(1 << 20);
    waiting(&mut states, node(1), picture());
    states.pin(&[node(1)]);
    assert!(states.park(node(1)).is_none(), "it is wanted");
    assert!(!states.is_held(node(1)));

    let mut states = CubeStates::new();
    made_for_the_warm_cut(&mut states, node(1), picture());
    assert!(states.park(node(1)).is_none(), "no budget: nothing is held");
    assert!(!states.is_held(node(1)));
}

#[test]
fn a_parked_picture_is_ready_for_the_planner_and_shown_by_a_move_when_it_is_wanted() {
    let mut states = CubeStates::new();
    states.set_budget(1 << 20);
    made_for_the_warm_cut(&mut states, node(1), picture());
    states.park(node(1));
    states.want(&[node(1)]);
    assert!(states.is_ready(node(1), 0));
    assert!(
        !states.needs_making(node(1), 0),
        "wanted and pinned, and held of this version"
    );
    assert!(states.reveal_node(node(1)), "the renderer shows its mesh");
    assert!(states.shown().contains(&node(1)));
    assert_eq!(states.held_pictures().nodes, 0);
    assert_consistent(&states);
}

#[test]
fn what_is_pinned_is_not_let_go_of_over_the_budget_and_is_when_it_is_not_pinned() {
    let mut states = CubeStates::new();
    states.set_budget(800);
    for x in 1..=3 {
        made_for_the_warm_cut(&mut states, node(x), picture_of(100));
        states.park(node(x));
    }
    assert_eq!(states.held_pictures().bytes, 2400);
    assert!(
        states.evict_over_budget().is_empty(),
        "all three are pinned: the budget is passed by them"
    );
    // The oldest is not pinned any more, and goes; then nothing is pinned, and the next oldest goes, which is enough.
    states.pin(&[node(2), node(3)]);
    assert_eq!(states.evict_over_budget(), [node(1)]);
    assert_eq!(states.held_pictures().nodes, 2);
    states.pin(&[]);
    assert_eq!(states.evict_over_budget(), [node(2)]);
    assert_eq!(states.held_pictures().nodes, 1);
    assert_consistent(&states);
}

#[test]
fn a_node_that_is_shown_and_taken_down_while_pinned_stays_held_and_clearing_forgets_the_pins() {
    let mut states = CubeStates::new();
    states.set_budget(100);
    states.pin(&[node(1)]);
    states.want(&[node(1)]);
    states.start_making(node(1), 0);
    states.finish_making(node(1), 0, 0, picture_of(100));
    states.show_node(node(1));
    states.want(&[]);
    assert_eq!(states.hold_node(node(1), 0), Taken::Hidden);
    assert!(
        states.evict_over_budget().is_empty(),
        "800 bytes against 100, and it is pinned"
    );
    assert!(states.is_held(node(1)));
    assert_consistent(&states);

    states.clear();
    assert!(!states.has_node(node(1)));
    assert!(!states.is_pinned(node(1)));
}

#[test]
fn a_pinned_picture_of_data_that_has_changed_is_made_again_and_an_unpinned_one_waits() {
    let mut states = CubeStates::new();
    states.set_budget(1 << 20);
    made_for_the_warm_cut(&mut states, node(1), picture());
    states.park(node(1));
    assert!(
        states.needs_making(node(1), 1),
        "pinned and held of version 0"
    );
    assert_eq!(states.drop_stale_held(node(1), 1), Taken::Removed);
    assert!(!states.is_held(node(1)));
    assert!(states.needs_making(node(1), 1));

    // Not pinned: held of old data, it is made again only when it is to be made.
    let mut states = CubeStates::new();
    states.set_budget(1 << 20);
    made_for_the_warm_cut(&mut states, node(1), picture());
    states.park(node(1));
    states.pin(&[]);
    assert!(!states.needs_making(node(1), 1));
}

#[test]
fn pictures_made_for_the_warm_cut_alone_are_let_go_of_when_nothing_is_pinned_and_a_picture_that_was_shown_is_not()
 {
    let mut states = CubeStates::new();
    states.set_budget(1 << 20);
    made_for_the_warm_cut(&mut states, node(1), picture_of(100));
    made_for_the_warm_cut(&mut states, node(2), PackedBrickMesh::default());
    states.park(node(1));
    states.park(node(2));
    // A picture that is held because it was shown and taken down.
    shown_with_budget(&mut states, node(3), picture_of(50), 1 << 20);
    states.want(&[]);
    assert_eq!(states.hold_node(node(3), 0), Taken::Hidden);
    let held_before = states.held_pictures();
    assert_eq!(held_before.nodes, 3);

    states.pin(&[]);
    assert_eq!(
        states.release_parked(),
        [node(1)],
        "the renderer holds a mesh of the first only: the second had nothing to see"
    );
    assert!(!states.has_node(node(1)) && !states.has_node(node(2)));
    assert!(states.is_held(node(3)), "that picture was the view's own");
    assert_eq!(
        states.held_pictures(),
        HeldPictures {
            nodes: 1,
            with_mesh: 1,
            bytes: 400
        }
    );
    assert_eq!(states.uploaded_nodes(), [node(3)]);
    assert!(states.release_parked().is_empty(), "once");
    assert_consistent(&states);
}

#[test]
fn a_parked_picture_that_was_shown_and_taken_down_again_is_held_like_any_other() {
    let mut states = CubeStates::new();
    states.set_budget(1 << 20);
    made_for_the_warm_cut(&mut states, node(1), picture_of(100));
    states.park(node(1));
    states.want(&[node(1)]);
    assert!(states.reveal_node(node(1)));
    states.want(&[]);
    assert_eq!(states.hold_node(node(1), 0), Taken::Hidden);
    states.pin(&[]);
    assert!(states.release_parked().is_empty());
    assert!(states.is_held(node(1)));
    assert_consistent(&states);
}

#[test]
fn a_parked_picture_of_a_cube_that_is_wanted_or_still_pinned_is_not_let_go_of() {
    let mut states = CubeStates::new();
    states.set_budget(1 << 20);
    made_for_the_warm_cut(&mut states, node(1), picture_of(100));
    made_for_the_warm_cut(&mut states, node(2), picture_of(100));
    states.park(node(1));
    states.park(node(2));
    // Both pinned: nothing is let go of.
    assert!(states.release_parked().is_empty());
    // One is wanted, and nothing is pinned: that one stays, to be shown.
    states.want(&[node(1)]);
    states.pin(&[]);
    assert_eq!(states.release_parked(), [node(2)]);
    assert!(states.is_held(node(1)) && !states.has_node(node(2)));
    assert_consistent(&states);
}

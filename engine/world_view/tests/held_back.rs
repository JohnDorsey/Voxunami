//! The mesher that holds its pictures back until it is asked to show them (`docs/specs/lod-spec.md`, 3.6): the bricks a coordinator swaps with coarse nodes.

use coarse_cells::FineRegion;
use voxel_space::BrickCoordinate;
use world_storage::test_support::{brick, id};
use world_view::{MeshUpdate, ViewChannels, WorldMesher};

mod common;

use common::{Scene, scene};

/// The bricks of a region that are to be drawn as bricks, in the order they are looked at.
fn wanted_around(centre: BrickCoordinate, radius: u32) -> Vec<BrickCoordinate> {
    let region = FineRegion::new(centre, radius);
    let reach = i128::from(radius) + 3;
    let mut wanted = Vec::new();
    for x in -reach..=reach {
        for y in -reach..=reach {
            for z in -reach..=reach {
                let at = BrickCoordinate::new(centre.x() + x, centre.y() + y, centre.z() + z);
                if region.wants_brick(at) {
                    wanted.push(at);
                }
            }
        }
    }
    wanted
}

fn held_back_mesher(scene: &Scene, radius: i128) -> WorldMesher {
    scene.mesher(radius).with_held_back_display()
}

#[test]
fn bricks_are_meshed_and_kept_ready_and_nothing_is_shown_until_it_is_asked_for() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let mut mesher = held_back_mesher(&scene, 3);
    let updates = mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(updates.is_empty(), "{updates:?}");

    // Every brick the region wants is ready, air or ground, and no other is.
    let wanted = wanted_around(brick(0, 0, 0), 3);
    for at in &wanted {
        assert!(mesher.is_ready(*at), "{at:?}");
    }
    for x in -8..=8 {
        for y in -8..=8 {
            for z in -8..=8 {
                let at = brick(x, y, z);
                assert_eq!(mesher.is_ready(at), wanted.contains(&at), "{at:?}");
            }
        }
    }
    assert!(!mesher.is_shown(brick(0, 0, 0)));
}

#[test]
fn a_brick_is_shown_by_asking_and_hidden_by_asking_and_air_has_nothing_to_show() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let mut mesher = held_back_mesher(&scene, 3);
    mesher.settle(scene.world.main(), brick(0, 0, 0));

    // The ground at y = 0 has a mesh; the sky at y = 3 has none, though it is ready.
    let ground = brick(0, 0, 0);
    let sky = brick(0, 3, 0);
    assert!(mesher.is_ready(sky));
    let shown = mesher.show(ground);
    assert!(
        matches!(&shown, Some(MeshUpdate::Upload { coordinate, mesh }) if *coordinate == ground && !mesh.is_empty()),
        "{shown:?}"
    );
    assert!(mesher.is_shown(ground));
    assert!(!mesher.is_ready(ground), "its picture was handed out");
    assert_eq!(mesher.show(sky), None);
    assert!(
        mesher.is_shown(sky),
        "shown, though there is nothing to see"
    );
    assert_eq!(mesher.show(sky), None);

    assert_eq!(mesher.hide(ground), Some(MeshUpdate::Remove(ground)));
    assert!(!mesher.is_shown(ground));
    assert_eq!(mesher.hide(ground), None);
    assert_eq!(mesher.hide(sky), None, "nothing was uploaded for it");
    assert!(!mesher.is_shown(sky));
}

#[test]
fn a_brick_that_is_not_ready_cannot_be_shown() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let mut mesher = held_back_mesher(&scene, 3);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    let far = brick(6, 0, 0);
    assert!(!mesher.is_ready(far));
    assert_eq!(mesher.show(far), None);
    assert!(!mesher.is_shown(far));
}

#[test]
fn an_edit_to_a_shown_brick_is_sent_at_once_and_one_to_a_brick_not_shown_waits_to_be_asked_for() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let mut mesher = held_back_mesher(&scene, 3);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    let shown = brick(0, 0, 0);
    let waiting = brick(1, 0, 0);
    mesher.show(shown);
    let waiting_before = mesher.show(waiting).unwrap();
    mesher.hide(waiting);
    // (It was handed out, so it has to be meshed again to be ready.)
    mesher.brick_changed(waiting);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(mesher.is_ready(waiting));

    // Dig at the +x edge of the first: its own mesh changes, and the neighbour's, which is not shown, is kept for later.
    let edited = scene.edit([31, 9, 5], "base_blocks:air");
    mesher.brick_changed(edited);
    let updates = mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert_eq!(updates.len(), 1, "{updates:?}");
    assert!(
        matches!(&updates[0], MeshUpdate::Upload { coordinate, .. } if *coordinate == shown),
        "{updates:?}"
    );
    assert!(mesher.is_ready(waiting), "meshed again, with the new wall");
    let waiting_after = mesher.show(waiting).unwrap();
    assert_ne!(waiting_after, waiting_before);
}

#[test]
fn a_brick_that_is_edited_before_it_is_asked_for_is_not_ready_until_meshed_again() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let mut mesher = held_back_mesher(&scene, 3);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(mesher.is_ready(brick(2, 0, 0)));
    let edited = scene.edit([64 + 5, 9, 5], "base_blocks:air");
    mesher.brick_changed(edited);
    assert!(
        !mesher.is_ready(brick(2, 0, 0)),
        "the picture it has is of the old bricks"
    );
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(mesher.is_ready(brick(2, 0, 0)));
}

#[test]
fn a_viewer_who_moves_gets_the_new_bricks_ready_and_the_ones_shown_stay_shown_until_hidden() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    let mut mesher = held_back_mesher(&scene, 3);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    let behind = brick(-3, 0, 0);
    assert!(mesher.show(behind).is_some());

    let updates = mesher.settle(scene.world.main(), brick(10, 0, 0));
    assert!(updates.is_empty(), "{updates:?}");
    assert!(mesher.is_ready(brick(10, 0, 0)));
    assert!(!mesher.is_ready(brick(0, 0, 0)), "no longer wanted");
    assert!(mesher.is_shown(behind), "it is the coordinator's to hide");
    assert_eq!(mesher.hide(behind), Some(MeshUpdate::Remove(behind)));
}

#[test]
fn what_is_loaded_for_the_neighbours_of_the_wanted_bricks_is_unloaded_when_far() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 40);
    let mut mesher = held_back_mesher(&scene, 3);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    let loaded = mesher.loaded_brick_count();
    mesher.settle(scene.world.main(), brick(30, 0, 0));
    assert!(mesher.loaded_brick_count() <= loaded + 40);
}

#[test]
fn a_reset_forgets_what_was_ready_and_reports_what_was_shown() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let mut mesher = held_back_mesher(&scene, 3);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    mesher.show(brick(0, 0, 0)).unwrap();
    let removed = mesher.reset();
    assert_eq!(removed, [MeshUpdate::Remove(brick(0, 0, 0))]);
    assert!(!mesher.is_shown(brick(0, 0, 0)));
    assert!(!mesher.is_ready(brick(1, 0, 0)));
}

#[test]
fn bricks_that_were_not_held_back_behave_as_they_did() {
    // Without the mode nothing of the above exists: the pictures are sent as soon as they are made.
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let mut mesher = scene.mesher(3);
    let updates = mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(!updates.is_empty());
    assert!(!mesher.is_ready(brick(0, 0, 0)));
    assert_eq!(mesher.show(brick(0, 0, 0)), None);
}

#[test]
fn changing_what_the_pictures_are_drawn_from_makes_the_waiting_ones_stale_and_they_are_made_again()
{
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let mut mesher = held_back_mesher(&scene, 3);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(mesher.is_ready(brick(0, 0, 0)));
    let temperature = scene
        .world
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:temperature"))
        .unwrap();
    mesher.set_channels(ViewChannels::new(scene.material, None, Some(temperature)));
    assert!(
        !mesher.is_ready(brick(0, 0, 0)),
        "drawn from the old channels"
    );
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(mesher.is_ready(brick(0, 0, 0)));
}
